fn main() {
    assert_eq!(std::env::args().nth(1).as_deref(), Some("--version"));
    println!("claude 2.1.251");
}
