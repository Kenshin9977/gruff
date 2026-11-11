// question: is a lone `*/`, with no block comment open, accepted by the compiler?
// unsupported: a lone `*/` is not a token of the language: the compiler refuses the file (bad syntax)
// expect compile error: bad syntax
// evidence: lexed as `*` then `/`, so the statement is an incomplete expression; the T7 mod tools linker says "syntax error, unexpected TOKEN_MULTIPLY" on the same form
// vs t7: same refusal
// note: this is what makes a file whose author assumed nested comments unloadable, the final `*/` being left over with nothing to close

main()
{
	iprintln("start");
	*/
	iprintln("after");
}
