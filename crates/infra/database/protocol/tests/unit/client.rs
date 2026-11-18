use std::io::BufReader;

use super::*;

/// A connection whose driver is played by the test: what the editor writes comes out of
/// the first, the driver's answers and error output go into the others.
struct Played {
    connection: Connection,
    requests: BufReader<std::io::PipeReader>,
    answers: std::io::PipeWriter,
    errors: std::io::PipeWriter,
}

fn played() -> Played {
    let (requests, editor_writes) = std::io::pipe().unwrap();
    let (editor_reads, answers) = std::io::pipe().unwrap();
    let (errors_read, errors) = std::io::pipe().unwrap();
    let connection = Connection::new(
        "Played",
        Transport {
            input: Box::new(editor_reads),
            output: Box::new(editor_writes),
            errors: Some(Box::new(errors_read)),
            process: None,
        },
    )
    .unwrap();
    Played { connection, requests: BufReader::new(requests), answers, errors }
}

fn wait_for(what: impl Fn() -> bool) {
    let started = Instant::now();
    while !what() {
        assert!(started.elapsed() < Duration::from_secs(5), "in time");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_password_is_taken_out_of_the_drivers_error_output_and_only_the_last_lines_kept() {
    let mut p = played();
    p.connection.hide(&Secret::new("s3cret!"));
    writeln!(p.errors, "connecting with s3cret! to db").unwrap();
    wait_for(|| !p.connection.log().is_empty());
    assert_eq!(p.connection.log(), vec!["connecting with •••• to db".to_owned()]);
    for n in 0..LOG_LINES + 10 {
        writeln!(p.errors, "line {n}").unwrap();
    }
    wait_for(|| p.connection.log().last().is_some_and(|l| l == &format!("line {}", LOG_LINES + 9)));
    assert_eq!(p.connection.log().len(), LOG_LINES);
}

#[test]
fn answers_find_their_requests_and_one_not_in_the_protocol_ends_the_connection() {
    let mut p = played();
    let asking = std::thread::spawn(move || {
        let tables: Result<Vec<dto::TableInfo>, DbError> = p.connection.call(
            method::TABLES,
            &dto::PathParams::default(),
            None,
            Duration::from_secs(5),
            (1, 1),
        );
        (p.connection, tables)
    });
    let request = wire::read_frame(&mut p.requests).unwrap().unwrap();
    let request: wire::Incoming<'_> = serde_json::from_slice(&request).unwrap();
    assert_eq!(request.method, Some(method::TABLES));
    let id = request.id.unwrap();
    // A notification of its own and an answer to nobody: ignored.
    wire::write_frame(&mut p.answers, br#"{"jsonrpc":"2.0","method":"hello"}"#).unwrap();
    wire::write_frame(&mut p.answers, br#"{"jsonrpc":"2.0","id":999,"result":[]}"#).unwrap();
    let answer = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":[{{"table":{{"name":"t"}}}}]}}"#);
    wire::write_frame(&mut p.answers, answer.as_bytes()).unwrap();
    let (connection, tables) = asking.join().unwrap();
    assert_eq!(tables.unwrap()[0].table.name, "t");
    // An answer of the wrong shape: the protocol is broken, the connection ends.
    let asking = std::thread::spawn(move || {
        let count: Result<u64, DbError> =
            connection.call(method::COUNT, &(), None, Duration::from_secs(5), (1, 1));
        (connection, count)
    });
    let request = wire::read_frame(&mut p.requests).unwrap().unwrap();
    let id = serde_json::from_slice::<wire::Incoming<'_>>(&request).unwrap().id.unwrap();
    let answer = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":"many"}}"#);
    wire::write_frame(&mut p.answers, answer.as_bytes()).unwrap();
    let (connection, count) = asking.join().unwrap();
    assert!(matches!(count, Err(DbError::Driver(DriverFailure::Stopped { .. }))), "{count:?}");
    assert!(connection.ended());
    let again: Result<u64, DbError> =
        connection.call(method::COUNT, &(), None, Duration::from_secs(5), (1, 1));
    assert!(matches!(again, Err(DbError::Driver(DriverFailure::Stopped { .. }))));
}

#[test]
fn a_cancel_is_sent_once_and_the_closed_connection_tells_the_driver_to_exit() {
    let mut p = played();
    let cancel = Cancel::default();
    cancel.cancel();
    let asking = std::thread::spawn(move || {
        let read: Result<dto::Rows, DbError> =
            p.connection.call(method::READ, &(), Some(&cancel), Duration::from_secs(5), (1, 1));
        (p.connection, read)
    });
    let request = wire::read_frame(&mut p.requests).unwrap().unwrap();
    let id = serde_json::from_slice::<wire::Incoming<'_>>(&request).unwrap().id.unwrap();
    let note = wire::read_frame(&mut p.requests).unwrap().unwrap();
    let note: wire::Incoming<'_> = serde_json::from_slice(&note).unwrap();
    assert_eq!((note.method, note.id), (Some(method::CANCEL), None));
    let asked: dto::CancelParams = serde_json::from_str(note.params.unwrap().get()).unwrap();
    assert_eq!(asked.id, id);
    let answer = format!(r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":4,"message":"x"}}}}"#);
    wire::write_frame(&mut p.answers, answer.as_bytes()).unwrap();
    let (connection, read) = asking.join().unwrap();
    assert_eq!(read, Err(DbError::Cancelled));
    drop(connection);
    let exit = wire::read_frame(&mut p.requests).unwrap().unwrap();
    let exit: wire::Incoming<'_> = serde_json::from_slice(&exit).unwrap();
    assert_eq!(exit.method, Some(method::EXIT));
    assert_eq!(wire::read_frame(&mut p.requests).unwrap(), None, "its input closed");
}
