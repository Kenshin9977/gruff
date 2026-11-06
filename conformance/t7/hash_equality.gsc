// question: do two hashes of names differing only in case compare equal (the hash lowercases), and does a string equal the hash of itself?
// fact: == between two hash values, or a string and a hash, is a non-terminal script error and yields false
// expect: f0 T0 script error: pair 'hash' and 'hash' has unmatching types 'hash' and 'hash'
// expect: f0 T0 hash case=0
// expect: f0 T0 script error: pair 'goaway' and 'hash' has unmatching types 'string' and 'hash'
// expect: f0 T0 string vs hash=0
#namespace zz_test;

function main()
{
	a = #"GoAway";
	b = #"goaway";
	logprint("hash case=" + (a == b));
	c = "goaway";
	logprint("string vs hash=" + (c == b));
}
