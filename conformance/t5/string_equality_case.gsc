// question: is == on strings case-sensitive?
// fact: == on strings is case-sensitive
// vs t6: same
// expect: f0 T0 same=1 case=0

main()
{
	a = "abc";
	b = "abc";
	c = "ABC";
	logprint("same=" + (a == b) + " case=" + (a == c));
}
