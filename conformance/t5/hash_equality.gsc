// question: do two hash literals differing only in case compare equal, and does a string equal a hash?
// fact: a hash literal is an int computed at compile time (case-insensitive): #"GoAway" == #"goaway" is true; a string == a hash is a non-terminal script error (string vs int) whose result is undefined
// vs t6: T6 hashes are their own type: hash == hash is a script error
// expect: f0 T0 hash case=1
// expect: f0 T0 script error: pair 'goaway' and '3290477' has unmatching types 'string' and 'int'
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
