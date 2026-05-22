// crap-rs#224: byte-offset -> (line, column) conversion precision. The
// `if` below is indented four spaces, so its IfBranch contributor sits
// at a known 1-based column (5). An off-by-one or operator swap in
// `byte_to_line_col` moves that column off 5 and is caught.
export function precise(a: number): number {
    if (a > 0) {
        return 1;
    }
    return 0;
}
