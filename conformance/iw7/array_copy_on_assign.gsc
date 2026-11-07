// question: b = a for an array, then b is changed: does a change too (reference) or not (copy)?
// fact: b = a then a write through b leaves a unchanged: arrays are values (copied when written while shared)
// expect: f0 T0 a[0]=1 b[0]=2 a.size=1 b.size=2
// vs s1: same

main()
{
	a = [];
	a[0] = 1;
	b = a;
	b[0] = 2;
	b[1] = 3;
	logprint("a[0]=" + a[0] + " b[0]=" + b[0] + " a.size=" + a.size + " b.size=" + b.size);
}
