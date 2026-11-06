// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// expect: f0 T0 a[0]=99 a.size=2
// fact: an array passed to a &name parameter is shared with the caller
// claim: bo3-gsc-language-doc (agrees): a &name parameter takes the array by reference
// claim: gscode-game-profiles (agrees): BO3 passes arrays by reference
// evidence: static: SafeCreateLocalVariables (0x12ce890): with flag 0x1 the pointer is kept; a non-array argument is Scr_Error 'Var ByRef only supported for array types'
#namespace zz_test;

function change(&arr)
{
	arr[0] = 99;
	arr[1] = 100;
}

function main()
{
	a = [];
	a[0] = 1;
	change(a);
	logprint("a[0]=" + a[0] + " a.size=" + a.size);
}
