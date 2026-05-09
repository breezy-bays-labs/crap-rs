// Fixture: all supported control flow constructs.
// Used by complexity walker tests.

use std::collections::HashMap;

/// Exercises: if, else if, while, for, match, loop, ?, &&, ||, async, closure.
/// Each construct is present for walker coverage; exact counts verified in tests.
pub fn kitchen_sink(items: &[i32], limit: usize) -> Result<Vec<i32>, String> {
    let mut result = Vec::new();
    let mut i = 0;

    // while (+1 + nesting 0)
    while i < items.len() {
        let item = items[i];

        // if (+1 + nesting 1)
        if item > 0 {
            // for (+1 + nesting 2)
            for j in 0..item {
                // if (+1 + nesting 3) with && (+1)
                if j > 0 && j < limit as i32 {
                    result.push(j);
                }
            }
        // else if (+1 continuation)
        } else if item == 0 {
            // match (+1 + nesting 2)
            match result.len() {
                0 => result.push(0),
                1..=5 => result.push(1),
                _ => {}
            }
        }
        // else is +0 cognitive

        i += 1;
    }

    Ok(result)
}

/// Function using the ? operator — adds +1 cognitive, +1 cyclomatic.
pub fn with_try_operator(input: &str) -> Result<usize, String> {
    let parsed: usize = input.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    Ok(parsed * 2)
}

/// Async function — should be detected with correct span.
pub async fn async_fetch(url: &str) -> Result<String, String> {
    if url.is_empty() {
        Err("empty url".to_string())
    } else {
        Ok(url.to_string())
    }
}

/// Function with a closure — closure complexity counts toward parent.
pub fn with_closure(items: &[i32]) -> Vec<i32> {
    items
        .iter()
        .filter(|&&x| x > 0 && x < 100)
        .copied()
        .collect()
}

/// Simple loop with break.
pub fn loop_with_break(max: usize) -> usize {
    let mut count = 0;
    loop {
        if count >= max {
            break;
        }
        count += 1;
    }
    count
}

/// let...else early exit — cognitive: base(1) + let-else(+1+0) = 2, cyclomatic: base(1) + let-else(+1) = 2.
pub fn let_else_early_exit(input: Option<i32>) -> i32 {
    let Some(value) = input else {
        return 0;
    };
    value * 2
}

/// Chained ? operators — each ? adds +1, and inner expressions are visited.
/// cognitive: base(1) + ?(+1) + ?(+1) = 3, cyclomatic: base(1) + ?(+1) + ?(+1) = 3.
pub fn chained_try(input: &str) -> Result<usize, String> {
    let parsed: usize = input.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    let doubled = parsed.checked_mul(2).ok_or("overflow".to_string())?;
    Ok(doubled)
}

/// match with ? in scrutinee — the ? inside the match input is counted.
/// cognitive: base(1) + match(+1+0) + ?(+1) = 3, cyclomatic: base(1) + match-arms + ?(+1).
pub fn match_with_try_scrutinee(input: &str) -> Result<&str, String> {
    match input.parse::<i32>().map_err(|e| e.to_string())? {
        0 => Ok("zero"),
        1..=100 => Ok("positive"),
        _ => Ok("other"),
    }
}

/// for loop with ? in iterator — the ? inside the iterator expression is counted.
/// cognitive: base(1) + for(+1+0) + ?(+1) = 3, cyclomatic: base(1) + for(+1) + ?(+1) = 3.
pub fn for_with_try_iterator(input: &str) -> Result<usize, String> {
    let mut total = 0;
    for ch in input.parse::<String>().map_err(|e| e.to_string())?.chars() {
        if ch.is_alphabetic() {
            total += 1;
        }
    }
    Ok(total)
}

/// Trait with a default method — should be found by the walker.
pub trait Describable {
    fn name(&self) -> &str;

    /// Default method with branching — cognitive: base(1) + if(+1+0) = 2.
    fn describe(&self) -> String {
        if self.name().is_empty() {
            "<unnamed>".to_string()
        } else {
            format!("Item: {}", self.name())
        }
    }
}
