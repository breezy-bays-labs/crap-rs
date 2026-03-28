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
