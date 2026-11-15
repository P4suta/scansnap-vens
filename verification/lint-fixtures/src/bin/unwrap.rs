fn value(input: Option<u8>) -> u8 {
    input.unwrap()
}

fn main() {
    let _value = value(Some(1));
}
