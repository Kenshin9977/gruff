// question: is == on strings case-sensitive?
// expect: f0 T0 same=1 case=0
// fact: == on strings is case-sensitive
// vs t7: same

main()
{
	a = "abc";
	b = "abc";
	c = "ABC";
	iprintln("same=" + (a == b) + " case=" + (a == c));
}
