// crap-rs#224: class method names resolved from non-identifier
// property keys — a private identifier, a string literal, and a
// numeric literal (the three `property_key_name` match arms). A
// dropped arm renames the method to the `<computed>` sentinel.
export class KeyedMethods {
  #secret(): number {
    return 1;
  }

  "string method"(): number {
    return 2;
  }

  42(): number {
    return 3;
  }
}
