// question: do two hashes of names differing only in case compare equal (the hash lowercases), and does a string equal the hash of itself?
// fact: hashes are compared as values: #"GoAway" == #"goaway" (the hash lowercases), and a string == its own hash is true, with no error
// vs t7: T7: == between two hashes, or a string and a hash, is a script error ('unmatching types') and yields false
// evidence: static: the equality evaluator (0x275bbe0) first coerces the pair (0x2760230: int/float, strings, hashes) and compares only then
// expect: f0 T0 hash case=1
// expect: f0 T0 string vs hash=1
#namespace zz_test;

function main()
{
	a = #"GoAway";
	b = #"goaway";
	iprintln("hash case=" + (a == b));
	c = "goaway";
	iprintln("string vs hash=" + (c == b));
}
