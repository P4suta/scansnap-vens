enum Mode {
    Simplex,
    Duplex,
    Card,
}

fn sides(mode: &Mode) -> u8 {
    match mode {
        Mode::Duplex => 2,
        _ => 1,
    }
}

fn main() {
    let _simplex = sides(&Mode::Simplex);
    let _duplex = sides(&Mode::Duplex);
    let _card = sides(&Mode::Card);
}
