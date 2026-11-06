// question: how many server frames do wait 0.05, wait 0.1 and wait 0.15 take?
// fact: with 50 ms server frames, wait 0.05 / 0.1 / 0.15 take 1 / 2 / 3 frames (waits are in milliseconds against level time)
// vs t8: same
// expect: f0 T0 start
// expect: f1 T0 after 0.05
// expect: f3 T0 after 0.1
// expect: f6 T0 after 0.15
#namespace zz_test;

function main()
{
	iprintln("start");
	wait 0.05;
	iprintln("after 0.05");
	wait 0.1;
	iprintln("after 0.1");
	wait 0.15;
	iprintln("after 0.15");
}
