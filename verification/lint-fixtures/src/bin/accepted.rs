fn first(values: &[u8]) -> Option<u8> {
    values.first().copied()
}

fn main() {
    let _first = first(&[1]);
}
