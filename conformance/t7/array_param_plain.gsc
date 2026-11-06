// question: an array passed to a plain parameter and changed in the callee: does the caller see the change?
// expect: f0 T0 a[0]=1 a.size=1
// fact: an array passed to a plain parameter is copied on entry (SafeCreateLocalVariables)
// claim: bo3-gsc-language-doc (agrees): arrays are passed by copy unless the parameter is declared &name
// claim: gscode-game-profiles (disagrees): BO3 passes arrays by reference
// evidence: static: SafeCreateLocalVariables (0x12ce890): for a parameter without flag 0x1 whose argument is a pointer to an array (type 0x19), AllocArray + CopyArray (0x12da210)
#namespace zz_test;

function change(arr)
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
