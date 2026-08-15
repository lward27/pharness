use eval_fixture::greet;

#[test]
fn greets_a_name() { assert_eq!(greet("Ada"), "Hello, Ada!"); }
