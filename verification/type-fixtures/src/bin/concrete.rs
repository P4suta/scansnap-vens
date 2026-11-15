fn main() {
    let value = String::from("concrete ownership");
    assert_eq!(value.len(), 18);
    let increment = |input: u8| input + 1;
    assert_eq!(increment(1), 2);
}
