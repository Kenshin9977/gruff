// question: t = s for a struct, then t.x is set: does s.x change (structs are references)?
// expect: f0 T0 s.x=1 same=1
// fact: structs are references: assignment and parameters share the object
// vs iw5: same

set_x(obj)
{
	obj.x = 1;
}

main()
{
	s = spawnstruct();
	t = s;
	set_x(t);
	logprint("s.x=" + s.x + " same=" + (s == t));
}
