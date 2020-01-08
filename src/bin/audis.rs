use audis;

#[macro_use]
extern crate clap;

use rand;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::env;

fn id() -> String {
    thread_rng().sample_iter(&Alphanumeric).take(30).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = clap_app!(audis =>
                         (version: "0.2.1")
                         (author: "James Hunt <james@niftylogic.com>")
                         (about: "Interact with an audit log, in Redis")
                         (@arg verbose: -v --verbose "Turn on verbose output")
                         (@arg host: -H --host +takes_value "URL of the Redis server to connect to")
                         (@subcommand subjects =>
                          (about: "List known subjects"))
                         (@subcommand retrieve =>
                          (about: "Print out an event log for one or more subjects")
                          (@arg subject: ... *))
                         (@subcommand log =>
                          (about: "Log an event against one or more subjects")
                          (@arg subject: -s --subject ... * +takes_value "The name of a subject to index this event against")
                          (@arg id: -i --id +takes_value "A unique ID to assign this event")
                          (@arg data: -d --data * +takes_value "The raw data to insert into the audit log"))
                         (@subcommand purge =>
                          (about: "Purge an event log, up to a last-known audit event")
                          (@arg subject: * +takes_value "The name of the subject / event log to purge")
                          (@arg to: -t --to * +takes_value "The event ID to purge up to (and including)"))
                         (@subcommand truncate =>
                          (about: "Truncate an event log such that it only includes a set number of events")
                          (@arg subject: * +takes_value "The name of the subject / event log to truncate")
                          (@arg n: -n --keep * +takes_value "How many audit events to keep")))
        .get_matches();

    let default_host = match env::var("AUDIS_HOST") {
        Ok(v) => v,
        Err(_) => "redis://127.0.0.1:6379".to_string(),
    };
    let c = audis::Client::connect(args.value_of("host").unwrap_or(&default_host))?;

    if let Some(_) = args.subcommand_matches("subjects") {
        for s in &c.subjects()? {
            println!("{}", s);
        }
    } else if let Some(args) = args.subcommand_matches("retrieve") {
        for s in args.values_of("subject").unwrap() {
            for e in c.retrieve(s)? {
                println!("{}: [{}] {}", s, e.id, e.data);
            }
        }
    } else if let Some(args) = args.subcommand_matches("log") {
        let id = match args.value_of("id") {
            Some(id) => id.to_string(),
            _ => id(),
        };
        c.log(&audis::Event {
            id: id,
            subjects: args.values_of_lossy("subject").unwrap(),
            data: args.value_of("data").unwrap().to_string(),
        })?;
    } else if let Some(args) = args.subcommand_matches("purge") {
        c.purge(
            &args.value_of("subject").unwrap().to_string(),
            &args.value_of("to").unwrap().to_string(),
        )?;
    }

    Ok(())
}
