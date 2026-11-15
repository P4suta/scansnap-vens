fn main() {
    let value: std::boxed::Box<u8> = std::boxed::Box::new(3);
    assert_eq!(*value, 3);
}
