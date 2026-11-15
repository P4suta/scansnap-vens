fn main() {
    let value: Box<u8> = Box::new(1);
    let _copy = *value;
}
