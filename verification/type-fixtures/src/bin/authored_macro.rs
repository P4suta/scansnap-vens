macro_rules! shared {
    () => {
        std::sync::Arc::new(3_u8)
    };
}

fn main() {
    let value = shared!();
    assert_eq!(*value, 3);
}
