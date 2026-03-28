// Fixture: simple top-level functions with varying complexity.
// Used by complexity walker tests.

/// Empty body — baseline complexity = 1 for both metrics.
pub fn empty_body() {}

/// Single if — cognitive: 1 (if+0 nesting), cyclomatic: 2 (base 1 + if 1).
pub fn single_if(x: i32) -> i32 {
    if x > 0 {
        x
    } else {
        -x
    }
}

/// Nested if — cognitive: 3 (outer if +1, inner if +1+1 nesting), cyclomatic: 3.
pub fn nested_if(x: i32, y: i32) -> i32 {
    if x > 0 {
        if y > 0 {
            x + y
        } else {
            x
        }
    } else {
        0
    }
}

/// Multiple returns with early return — cognitive: 1, cyclomatic: 2.
pub fn early_return(x: i32) -> i32 {
    if x < 0 {
        return 0;
    }
    x * 2
}
