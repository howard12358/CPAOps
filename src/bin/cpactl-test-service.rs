use std::env;
use std::fs;
use std::net::TcpListener;

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--help") {
        println!("cpactl test service");
        return;
    }
    let config = arguments
        .windows(2)
        .find(|arguments| arguments[0] == "-config" || arguments[0] == "-env")
        .map(|arguments| &arguments[1])
        .unwrap_or_else(|| panic!("missing service configuration path"));
    let contents = fs::read_to_string(config).expect("read service configuration");
    let port = contents
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("port:")
                .or_else(|| line.trim().strip_prefix("APP_PORT="))
                .and_then(|value| value.trim().parse::<u16>().ok())
        })
        .expect("service configuration contains port");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind test service port");
    for stream in listener.incoming() {
        drop(stream);
    }
}
