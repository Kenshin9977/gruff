// question: does a `/*` inside a block comment nest, so that the comment ends only at the matching `*/`?
// unsupported: T7 has no nested block comments: the comment ends at the first `*/` and the `*/` that follows it is a syntax error
// expect compile error: unexpected TOKEN_MULTIPLY
// evidence: the official mod tools linker points at the second `*/` and names the token it lexed there ("syntax error, unexpected TOKEN_MULTIPLY"), so the comment was already closed at the first one; the same file with a plain `/* a b */` compiles and traces start then after. Same refusal in iw3, t4 and t5, each with its own official compiler.
#namespace zz_test;

function main()
{
	logprint("start");
	/* a /* b */ */
	logprint("after");
}
