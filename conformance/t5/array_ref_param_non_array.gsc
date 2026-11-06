// question: a non-array value passed to a &name parameter
// vs t6: same: T6 has none either
// unsupported: the form does not exist in T5: T5 has no &name parameters; the game's own compiler refuses the script at load
// expect compile error: bad syntax

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
