# crap4ts

Rust-powered [CRAP (Change Risk Anti-Patterns)](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html) score analyzer for TypeScript and JavaScript. Combines [oxc](https://oxc.rs/)-driven AST complexity with [Istanbul](https://istanbul.js.org/) JSON coverage to find functions that are both complex and under-tested.

crap4ts is the TypeScript adapter for the `crap-rs` analyzer, distributed as a [napi-rs](https://napi.rs/) Node addon. It shares its CRAP formula, scorecard envelope, and reporter shapes with the Rust adapter (`crap4rs`), so a CRAP score means the same thing whether you analyze a TypeScript project or a Rust one. crap4ts measures complexity with the cyclomatic metric; crap4rs defaults to cognitive.

## Install

```sh
npm install crap4ts
# pre-release channel:
npm install crap4ts@rc
```

## Usage

```js
const { analyze } = require('crap4ts');

const json = analyze({
  sourceRoot: 'src',
  coveragePath: 'coverage/coverage-final.json',
  // Optional:
  // threshold: 15,        // default gate
  // metric: 'cyclomatic', // crap4ts is cyclomatic-only
});

const { result, diagnostics } = JSON.parse(json);
console.log(result.summary);
```

Generate `coverage-final.json` with Istanbul coverage enabled in your test runner (`jest --coverage`, `vitest --coverage`, `c8 --reporter=json`, etc.).

Use the istanbul coverage provider, not v8 — crap4ts consumes the Istanbul JSON shape only. For Vitest, set `coverage.provider: 'istanbul'`.

## Documentation

The CRAP score, the threshold gate, and the full reporter gallery (terminal, JSON, HTML — HTML ships today) are documented in the book:

- [Quick start (TypeScript)](https://breezy-bays-labs.github.io/crap-rs/book/quick-start.html)
- [Understanding CRAP](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html)

## License

MIT OR Apache-2.0.
