// question: do two hash literals differing only in case compare equal, and does a string equal a hash?
// fact: == between two hashes, or a string and a hash, is a non-terminal script error ('unmatching types') whose result is undefined (the concatenation that uses it errs again)
// vs t7: the same error, but the result is false (0)
// expect: f0 T0 script error: pair 'hash' and 'hash' has unmatching types 'hash' and 'hash'
// expect: f0 T0 script error: pair 'hash case=' and 'undefined' has unmatching types 'string' and 'undefined'
// expect: f0 T0 undefined
// expect: f0 T0 script error: pair 'goaway' and 'hash' has unmatching types 'string' and 'hash'
// expect: f0 T0 script error: pair 'string vs hash=' and 'undefined' has unmatching types 'string' and 'undefined'
// expect: f0 T0 undefined

main()
{
	a = #"GoAway";
	b = #"goaway";
	logprint("hash case=" + (a == b));
	c = "goaway";
	logprint("string vs hash=" + (c == b));
}
