// question: the copy made for a plain array parameter: shallow (inner arrays shared) or deep?
// fact: the copy made for a plain array parameter is deep: an inner array written through the copy is not seen by the caller
// vs t7: same
// evidence: static: SafeCreateLocalVariables (0x274a3d0): for a parameter without flag 0x1 whose argument is a pointer to an array (type 0x18), AllocArray (0x2758530) + CopyArray (0x2759520), as T7's 0x12ce890
// expect: f0 T0 inner=1 outer.size=1
#namespace zz_test;

function change(arr)
{
	arr[0][0] = 2;
	arr[1] = "new";
}

function main()
{
	a = [];
	a[0] = [];
	a[0][0] = 1;
	change(a);
	iprintln("inner=" + a[0][0] + " outer.size=" + a.size);
}
