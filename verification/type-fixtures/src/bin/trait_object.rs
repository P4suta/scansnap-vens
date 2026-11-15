fn value(input: &dyn std::fmt::Display) -> String {
    input.to_string()
}

fn main() {
    assert_eq!(value(&3), "3");
}
