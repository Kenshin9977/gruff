// question: does a `/*` inside a block comment nest, so that the comment ends only at the matching `*/`?
// unsupported: T5 has no nested block comments: the comment ends at the first `*/` and the `*/` that follows it is bad syntax
// expect compile error: bad syntax
// evidence: the same file with a plain `/* a b */` compiles and traces start then after, so the refusal is the nesting and not the shape of the test; same refusal in iw3, t4 and t7, each with its own official compiler or linker

main()
{
	logprint("start");
	/* a /* b */ */
	logprint("after");
}
