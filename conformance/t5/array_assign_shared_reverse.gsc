// question: after b = a, is a write through a seen through b (the other direction)?
// fact: after b = a, a write through a is not seen through b: a shared array is copied when written (copy on write), so b keeps the old empty array
// vs t6: same
// expect: f0 T0 script error: pair 'b.size=0 b[0]=' and 'undefined' has unmatching types 'string' and 'undefined'
// expect: f0 T0 undefined

main()
{
	a = [];
	b = a;
	a[0] = 7;
	logprint("b.size=" + b.size + " b[0]=" + b[0]);
}
