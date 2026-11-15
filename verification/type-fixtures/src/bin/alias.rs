type Shared<T> = std::sync::Arc<T>;

fn main() {
    let value: Shared<u8> = Shared::new(3);
    assert_eq!(*value, 3);
}
