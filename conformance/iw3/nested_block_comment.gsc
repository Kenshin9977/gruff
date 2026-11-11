// question: does a `/*` inside a block comment nest, so that the comment ends only at the matching `*/`?
// unsupported: IW3 has no nested block comments: the comment ends at the first `*/` and the `*/` that follows it is bad syntax
// expect compile error: bad syntax
// evidence: the same file with a plain `/* a b */` compiles and traces start then after, so the refusal is the nesting and not the shape of the test
// vs t4: same refusal, same compiler lineage
// vs t7: same refusal, the linker naming the token ("syntax error, unexpected TOKEN_MULTIPLY")

main()
{
	logprint("start");
	/* a /* b */ */
	logprint("after");
}
