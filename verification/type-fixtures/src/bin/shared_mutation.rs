fn main() {
    let value = std::cell::RefCell::new(3_u8);
    *value.borrow_mut() = 4;
}
