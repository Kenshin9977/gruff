// question: a non-array value passed to a &name parameter
// unsupported: T4 has no reference parameters: the game's compiler rejects `&v` in a parameter list (bad syntax)
// expect compile error: bad syntax
// vs t7: T7 raises 'Var ByRef only supported for array types', then its error recovery loops

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	iprintln("x=" + x);
}
