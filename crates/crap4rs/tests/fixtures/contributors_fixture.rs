// contributors_fixture.rs — functions for contributor golden tests.
// Each function isolates one construct for predictable contributor extraction.

pub fn empty_fn() -> i32 {
    42
}

pub fn single_if_fn(x: i32) -> i32 {
    if x > 0 {
        1
    } else {
        0
    }
}

pub fn nested_if_fn(x: i32, y: i32) -> i32 {
    if x > 0 {
        if y > 0 {
            1
        } else {
            0
        }
    } else {
        -1
    }
}

pub fn match_fn(x: u8) -> &'static str {
    match x {
        0 => "zero",
        1 => "one",
        2 => "two",
        _ => "other",
    }
}

pub fn try_fn() -> Result<i32, String> {
    let x: Result<i32, String> = Ok(1);
    Ok(x?)
}

pub fn let_else_fn(s: Option<i32>) -> i32 {
    let Some(n) = s else {
        return 0;
    };
    n
}

pub fn loop_fn() -> i32 {
    loop {
        return 1;
    }
}

pub fn for_loop_fn() -> i32 {
    let mut sum = 0;
    for i in 0..10 {
        sum += i;
    }
    sum
}

pub fn while_loop_fn(mut n: i32) -> i32 {
    while n > 0 {
        n -= 1;
    }
    n
}

pub fn logical_same_chain_fn(a: bool, b: bool, c: bool) -> bool {
    a && b && c
}

pub fn logical_op_switch_fn(a: bool, b: bool, c: bool) -> bool {
    a && b || c
}

pub fn loop_with_break_fn(n: i32) -> i32 {
    let mut i = 0;
    loop {
        if i >= n {
            break;
        }
        i += 1;
    }
    i
}

pub fn for_with_continue_fn() -> i32 {
    let mut sum = 0;
    for i in 0..10 {
        if i % 2 == 0 {
            continue;
        }
        sum += i;
    }
    sum
}

pub fn with_closure_fn(items: &[i32]) -> Vec<i32> {
    items.iter().filter(|&&x| x > 0).copied().collect()
}

pub fn sorted_by_line_fn(a: bool, n: i32) -> i32 {
    if a {
        for _i in 0..n {
            let _ = 1;
        }
    }
    0
}
