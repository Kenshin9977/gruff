// question: after b = a, is a write through a seen through b (the other direction)?
// fact: no: after b = a, a write through a leaves b as it was (b stays empty)
// expect: f0 T0 b.size=0 b[0] defined=0
// vs iw3: same

main()
{
	a = [];
	b = a;
	a[0] = 7;
	logprint("b.size=" + b.size + " b[0] defined=" + isdefined(b[0]));
}
