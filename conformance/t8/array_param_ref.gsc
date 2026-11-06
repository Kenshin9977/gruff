// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// fact: an array passed to a &name parameter is shared with the caller
// vs t7: same
// expect: f0 T0 a[0]=99 a.size=2
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
	iprintln("a[0]=" + a[0] + " a.size=" + a.size);
}
