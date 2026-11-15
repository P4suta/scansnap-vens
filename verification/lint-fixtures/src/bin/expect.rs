fn value(input: Option<u8>) -> u8 {
    input.expect("present")
}

fn main() {
    let _value = value(Some(1));
}
