extern crate block;

#[test]
fn stack_and_copied_blocks_invoke_the_captured_closure() {
    let captured = 7;
    let stack = block::ConcreteBlock::new(move |value: i32| captured + value);
    assert_eq!(unsafe { stack.call((5,)) }, 12);
    let copied = stack.copy();
    let retained = copied.clone();
    drop(copied);
    assert_eq!(unsafe { retained.call((9,)) }, 16);
}
