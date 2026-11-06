// question: b = a for an array, then b is changed: does a change too (reference) or not (copy)?
// fact: b = a then a write through b leaves a unchanged: arrays are values (copied when written while shared)
// expect: f0 T0 a[0]=1 b[0]=2 a.size=1 b.size=2
// evidence: static: the array-ref evaluator (0x516ff0) tests the array's reference count (+4) at 0x516fb5: when shared it drops its reference (0x514a40), allocates a new array (0x514990) and copies the elements (0x515040) before the write; the emulated run reaches 0x514990 from there between `b = a` and the first write through b
// vs t4: same

main()
{
	a = [];
	a[0] = 1;
	b = a;
	b[0] = 2;
	b[1] = 3;
	logprint("a[0]=" + a[0] + " b[0]=" + b[0] + " a.size=" + a.size + " b.size=" + b.size);
}
