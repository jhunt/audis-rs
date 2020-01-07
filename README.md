# audis

Audis is an implementation of a multi-index audit log,
built atop the Redis key-value store solution.

An audit log consists of zero or more Event objects,
indexed against one or more subjects, each.
The correspondence between Redis databases and audit
logs is 1:1 -- each audit log inhabits precisely one
Redis instance, and an instance can only house a single
audit log.

### Example: Logging Events

The simplest way to use audis is to point it at a Redis
instance and start logging to it:

```rust
extern crate audis;

fn main() {
    let client = audis::Client::connect("redis://127.0.0.1:6379").unwrap();

    client.log(&audis::Event{
        id: "foo1".to_string(),
        data: "{\"some\":\"data\"}".to_string(),
        subjects: vec![
            "system".to_string(),
            "user:42".to_string(),
        ],
    }).unwrap();

    // ... etc ...
}
```

### Retrieving The Audit Log

What good is an audit log if you can't ever review it?

```rust
extern crate audis;

fn main() {
    let client = audis::Client::connect("redis://127.0.0.1:6379").unwrap();

    for subject in &client.subjects().unwrap() {
        println!("## {} ######################", subject);
        for event in &client.retrieve(subject).unwrap() {
            println!("  {}", event.data);
        }
        println!("");
    }
}
```

### Distributed Audit Logging via Threads

A common pattern with audis is to delegate a single thread
to the task of shunting event logs into Redis, allowing other
threads to focus on their work, without being slowed down by
momentary hiccups in the auditing layer.

You can do this via the `background()` function, which returns
a buffered channel you can send events to, and the join handle
of the executing background thread:

```rust
extern crate audis;

fn main() {
    let client = audis::Client::connect("redis://127.0.0.1:6379").unwrap();

    // buffer 50 events
    let (tx, thread) = client.background(50).unwrap();

    tx.send(audis::Event{
        id: "foo1".to_string(),
        data: "{\"some\":\"data\"}".to_string(),
        subjects: vec![
            "system".to_string(),
            "user:42".to_string(),
        ],
    }).unwrap();

    // ... etc ...

    thread.join().unwrap();
}
```

### Implementation Details

Audis uses four (4) types of objects A) the events
themselves, B) reference counts for inserted events,
C) per-subject lists of events, sorted in insertion-order,
and D) a master list of all known subjects.

Each audit event is stored as an opaque blob, usually
JSON -- for the purposes of this library, the exact contents
of events is immaterial.  Each event gets its own Redis key,
derived from its globally unique ID, in the form `audit:$id`.
This makes retrieving events an _O(1)_ operation, using the
Redis `GET` command.

Accompanying each event object is a reference count, kept
under a parallel keying structure that appends `:ref` to the
main event object key.  These reference counts are integers
that track how many different subjects are currently still
referencing the given event.  For example, `audit:ae2:ref`
is the reference count key for `audit:ae2`.

Each subject in the audit log maintains its own list of
event IDs that are relevant to it.  These lists are stored
under keys derived from the subject itself.  Callers are
strongly urged to ensure that subject names are as unique
as they need to be for analysis.

Finally, a single Redis Set, called `subjects`, exists to
track the complete set of known subject strings.  This
facilitates discovery of the different subsets of the audit
log.

Here is some pseudocode for the insertion logic of the
`LOG(e)` operation, where `e` is an object:

```redis-pseudo-code
LOG(e):
    var id = $e[id]
    SETEX "audit:$id" $e[data]
    for s in $e[subjects]:
        SADD "subjects" "$s"
        RPUSH "$s" "$id"
        INCR "audit:$id:ref"
```

Technically speaking, `LOG(e)` runs in _O(n)_, linearly
to the number of subjects that the audit log event applies
to.  However, given that this `n` is usually very small
(almost always < 100), `LOG(e)` performs well.

`RETR(s)` is straightforward: iterate over the subject list
in Redis via `LRANGE` and then `GET` the referenced event
objects:

```redis-pseudo-code
RETR(s):
    var log = []
    for id in LRANGE "$s" 0 -1:
        $log.append( GET "audit:$id" )
    return $log
```

Since `LOG(e)` only ever adds to our audit log dataset,
and `RETR(s)` is a read-only operation, our Redis footprint
will forever grow, unless we define operations to clear out
old log entries.  Deleting parts of our audit log seems
wrong and counter-productive -- the whole point is to know
what happened!  Storage (especially memory), however, isn't
unlimited, and for debugging purposes (at least), audit log
events become less relevant over time.

For these reasons, we define two pruning operations:
`TRUNC(s,n)`, for truncating a subject eventset to the most
recent `n` audit events, and `PURGE(s,last)`, for deleting
a events from a subject eventset, until a given ID is found.
(That ID will also be removed, as well).

These two operations allow us to define a cleanup routine,
outside of the audis library, which can do things like
render and persist audit events to a log file in a filesystem
or external blobstore (i.e. S3), before ultimately deleting
them from Redis.

Here is the pseudo-code for `TRUNC(s,n)`:

```redis-pseudo-code
TRUNC(s,n):
    var end = 0 - n - 1
    for id in LRANGE "$s" 0 $end:
        LPOP "$s"
        DECR "audit:$id:ref"
        if GET "audit:$id:ref" <= 0:
            DEL "audit:$id:ref"
            DEL "audit:$id"
```

As events are truncated from the subject's index, the
associated reference counts are checked to determine if any
larger cleanup (via `DEL`) needs to be performed.

`PURGE(s,last)` is similar:

```redis-pseudo-code
PURGE(s,last):
    for id in LRANGE "$s" 0 -1:
        LPOP "$s"
        DECR "audit:$id:ref"
        if GET "audit:$id:ref" <= 0:
            DEL "audit:$id:ref"
            DEL "audit:$id"
        if $id == $last
            break
```

Both of these operations suffer from massive problems
when run concurrently with each other, or with other
calls to themselves.  A future version of this library
will correct this, by the judicious use of `LOCK()`/`UNLOCK()`
primitives implemented inside of the same Redis database.

