// bellwav OUT.wav [double]: write the rebuilt Apple bell (kq1_apple2::sound) to a WAV file.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    std::fs::write(&a[1], kq1_apple2::sound::wav(a.len() > 2, 44_100)).unwrap();
}
