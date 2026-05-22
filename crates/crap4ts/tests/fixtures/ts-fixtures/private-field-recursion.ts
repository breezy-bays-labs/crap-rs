// crap-rs#224: the object of a private-field access is walked in all
// three positions — assignment LHS, update operand, and a plain read
// (the PrivateFieldExpression arms under `visit_expression`'s
// AssignmentExpression / UpdateExpression / member-expression cases).
// The parenthesised ternary in the object position is the observable
// decision point that proves each arm recursed.
export class PrivateFieldHost {
  #v = 0;

  assign(flag: boolean): void {
    (flag ? this : this).#v = 1;
  }

  update(flag: boolean): void {
    (flag ? this : this).#v++;
  }

  read(flag: boolean): number {
    return (flag ? this : this).#v;
  }
}
