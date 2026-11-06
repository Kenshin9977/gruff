// question: a notify nobody waits for: no error, the caller goes on?
// fact: a notify nobody waits for is silent
// vs t8: same
// expect: f0 T0 before
// expect: f0 T0 after
#namespace zz_test;

function main()
{
	iprintln("before");
	level notify(#"nobody");
	iprintln("after");
}
