// question: an array passed to a plain parameter and changed in the callee: does the caller see the change?
// fact: an array passed to a plain parameter is copied on entry (SafeCreateLocalVariables)
// vs t7: same
// evidence: static: SafeCreateLocalVariables (0x274a3d0): for a parameter without flag 0x1 whose argument is a pointer to an array (type 0x18), AllocArray (0x2758530) + CopyArray (0x2759520), as T7's 0x12ce890
// expect: f0 T0 a[0]=1 a.size=1
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
	iprintln("a[0]=" + a[0] + " a.size=" + a.size);
}
