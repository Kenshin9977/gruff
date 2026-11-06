#namespace helpers;

function wait_a_bit()
{
	deeper();
}

function deeper()
{
	wait 0.05;
}

function pure()
{
	return 1 + 2;
}
