fn main() {
    let value = std::sync::Arc::new(3_u8);
    assert_eq!(*value, 3);
}
