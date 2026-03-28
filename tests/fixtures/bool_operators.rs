// Fixture: boolean operator sequence collapsing.
// Tests cognitive complexity counting for && and || chains.

/// Same-operator sequence: `a && b && c` = cognitive +1 for the whole sequence.
/// Cyclomatic: +3 (base 1, && +1, && +1).
pub fn same_sequence(a: bool, b: bool, c: bool) -> bool {
    a && b && c
}

/// Operator switch: `a && b || c` = cognitive +2 (&&-sequence +1, ||-switch +1).
/// Cyclomatic: +3 (base 1, && +1, || +1).
pub fn operator_switch(a: bool, b: bool, c: bool) -> bool {
    a && b || c
}

/// Mixed with if: cognitive = if(+1) + &&-sequence(+1) + ||-switch(+1) = 3 + nesting.
/// Actually: if(+1+0nesting) + &&(+1) + ||(+1) = 3 cognitive.
/// Cyclomatic: base(1) + if(+1) + &&(+1) + ||(+1) = 4.
pub fn bool_in_condition(a: bool, b: bool, c: bool) -> &'static str {
    if a && b || c {
        "yes"
    } else {
        "no"
    }
}

/// Long same-operator chain: `a || b || c || d` = cognitive +1 total.
/// Cyclomatic: base(1) + 3 = 4.
pub fn long_or_chain(a: bool, b: bool, c: bool, d: bool) -> bool {
    a || b || c || d
}

/// Alternating operators: `a && b || c && d` = cognitive +3 (&&, ||, &&).
/// Cyclomatic: base(1) + 3 = 4.
pub fn alternating_operators(a: bool, b: bool, c: bool, d: bool) -> bool {
    a && b || c && d
}
