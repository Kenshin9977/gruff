// question: does a `/*` inside a block comment nest, so that the comment ends only at the matching `*/`?
// unsupported: T4 has no nested block comments: the comment ends at the first `*/` and the `*/` that follows it is bad syntax
// expect compile error: bad syntax
// evidence: the same file with a plain `/* a b */` compiles and traces start then after, so the refusal is the nesting and not the shape of the test; the T7 mod tools linker refuses the same form naming the token ("syntax error, unexpected TOKEN_MULTIPLY" at the second `*/`)
// vs t7: same refusal; no game of this project's measured set (iw3, t4, t5, t7, each with its own official compiler or linker) nests block comments
// note: asked because a shipped World at War script looks written as if they nested (an outer comment over most of the file, an inner one over an older version of a function, and a final `*/`); measured here, they do not, and such a file is refused at load (see stray_block_comment_close)

main()
{
	iprintln("start");
	/* a /* b */ */
	iprintln("after");
}
