// question: after b = a, is a write through a seen through b (the other direction)?
// fact: assignment shares the array both ways
// expect: f0 T0 b.size=1 b[0]=7
#namespace zz_test;

function main()
{
	a = [];
	b = a;
	a[0] = 7;
	logprint("b.size=" + b.size + " b[0]=" + b[0]);
}
