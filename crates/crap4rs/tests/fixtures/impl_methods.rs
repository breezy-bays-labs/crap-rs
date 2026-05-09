// Fixture: struct with impl block for qualified name testing.
// Used by complexity walker tests.

pub struct Calculator {
    value: f64,
}

impl Calculator {
    /// Associated function (no self) — qualified: "Calculator::new".
    pub fn new() -> Self {
        Self { value: 0.0 }
    }

    /// Method with self — qualified: "Calculator::add".
    /// Complexity: cognitive 1, cyclomatic 1 (no branching).
    pub fn add(&mut self, x: f64) {
        self.value += x;
    }

    /// Method with branching — qualified: "Calculator::divide".
    /// Complexity: cognitive 1 (if+0), cyclomatic 2 (base+if).
    pub fn divide(&mut self, x: f64) -> Result<f64, &'static str> {
        if x == 0.0 {
            Err("division by zero")
        } else {
            self.value /= x;
            Ok(self.value)
        }
    }
}
