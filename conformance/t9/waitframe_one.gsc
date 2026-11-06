// question: how many server frames do waitframe(1) and waitframe(2) take, against wait 0.05?
// fact: waitframe(n) resumes n frames later, by the VM's frame counter; waitframe(1) and wait 0.05 both take one 50 ms frame
// vs t8: same
// expect: f0 T0 start
// expect: f1 T0 after waitframe 1
// expect: f3 T0 after waitframe 2
// expect: f4 T0 after wait 0.05
#namespace zz_test;

function main()
{
	iprintln("start");
	waitframe(1);
	iprintln("after waitframe 1");
	waitframe(2);
	iprintln("after waitframe 2");
	wait 0.05;
	iprintln("after wait 0.05");
}
