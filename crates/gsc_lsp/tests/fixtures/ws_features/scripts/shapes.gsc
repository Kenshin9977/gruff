#namespace shapes;

class cShape
{
	var area;
	var name;

	constructor()
	{
		area = 0;
	}

	function describe()
	{
		return name;
	}
}

class cSquare : cShape
{
	function describe()
	{
		area = 4;
		return "square";
	}
}

function make()
{
	s = new cSquare();
	return s;
}
