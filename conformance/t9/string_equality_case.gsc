// question: is == on strings case-sensitive?
// fact: == on strings is case-sensitive
// vs t8: same
// expect: f0 T0 same=1 case=0
#namespace zz_test;

function main()
{
	a = "abc";
	b = "abc";
	c = "ABC";
	iprintln("same=" + (a == b) + " case=" + (a == c));
}
