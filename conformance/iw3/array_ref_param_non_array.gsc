// question: a non-array value passed to a &name parameter
// unsupported: IW3 has no reference parameters: the game's compiler rejects `&v` in a parameter list (bad syntax)
// expect compile error: bad syntax
// vs t4: same (the form cannot exist in T4 either)

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
