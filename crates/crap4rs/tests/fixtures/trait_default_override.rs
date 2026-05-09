// Fixture: trait with default method body + concrete impl that overrides
// the same-named method. Pinning evidence for crap4rs#116 — the walker
// must emit two distinct FunctionComplexity entries (one per body) with
// disjoint spans, so contributors and ProposedSplits can never leak
// across the trait/impl boundary.

pub trait Greeter {
    fn name(&self) -> &str;

    /// Default body. Cognitive: base(1) + if(+1) = 2.
    fn greet(&self) -> String {
        if self.name().is_empty() {
            "anon".to_string()
        } else {
            format!("hello {}", self.name())
        }
    }
}

pub struct Casual {
    nick: String,
}

impl Greeter for Casual {
    fn name(&self) -> &str {
        &self.nick
    }

    /// Override with different branching. Cognitive: base(1) + if(+1+0) +
    /// else-if(+1+0) = 3 (else-if does not add nesting in cognitive
    /// complexity — distinct from a fully nested if).
    fn greet(&self) -> String {
        if self.nick.is_empty() {
            "yo".to_string()
        } else if self.nick.len() < 3 {
            format!("yo {}", self.nick)
        } else {
            format!("hey {}!", self.nick)
        }
    }
}
