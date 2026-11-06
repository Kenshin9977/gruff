// question: b = a for an array, then b is changed: does a change too (reference) or not (copy)?
// fact: b = a shares the array: a write through b is seen through a (no copy on assignment, no copy on write)
// expect: f0 T0 a[0]=2 b[0]=2 a.size=2 b.size=2
// evidence: static: ScrVar_GetArrayRef (0x12de5b0) allocates an array only when the variable is undefined and otherwise returns the shared pointer; no reference-count check, no copy
#namespace zz_test;

function main()
{
	a = [];
	a[0] = 1;
	b = a;
	b[0] = 2;
	b[1] = 3;
	logprint("a[0]=" + a[0] + " b[0]=" + b[0] + " a.size=" + a.size + " b.size=" + b.size);
}
