// question: a non-array value passed to a &name parameter
// fact: a non-array value given to a &name parameter is accepted silently: no script error, the callee runs and the caller's value is unchanged
// vs t8: T8 and T7: a script error ('Var ByRef only supported for array types' in T7), then an error loop in the VM's recovery
// expect: f0 T0 x=5
#namespace zz_test;

function change(&v)
{
}

function main()
{
	x = 5;
	change(x);
	iprintln("x=" + x);
}
