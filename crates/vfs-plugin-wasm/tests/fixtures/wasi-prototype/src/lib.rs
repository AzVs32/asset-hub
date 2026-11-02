wit_bindgen::generate!({
    inline: "package assethub:prototype; world probe { export call: func(request: list<u8>) -> list<u8>; }",
    world: "probe",
});
struct Probe;
impl Guest for Probe {
    fn call(request: Vec<u8>) -> Vec<u8> {
        use std::io::Read;
        let request = String::from_utf8(request).expect("UTF-8 request");
        if request == "cpu" {
            loop { std::hint::spin_loop(); }
        }
        if let Some(address) = request.strip_prefix("read:") {
            let mut stream = match std::net::TcpStream::connect(address) { Ok(stream) => stream, Err(error) => return format!("connect error: {error:?}").into_bytes() };
            let mut byte = [0];
            stream.read_exact(&mut byte).expect("read");
            return byte.to_vec();
        }
        std::env::var("PROTOTYPE_MARKER").expect("marker").into_bytes()
    }
}
export!(Probe);
