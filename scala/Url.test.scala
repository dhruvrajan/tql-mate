package tqlmate

class UrlTest extends munit.FunSuite:
  test("parses full URL"):
    val u = TypeDbUrl.parse("typedb://admin:secret@db.example:1730/mydb?tls=true").toOption.get
    assertEquals(u.username, "admin")
    assertEquals(u.password, "secret")
    assertEquals(u.host, "db.example")
    assertEquals(u.port, 1730)
    assertEquals(u.database, "mydb")
    assertEquals(u.tls, true)
    assertEquals(u.address, "db.example:1730")

  test("default port and no tls"):
    val u = TypeDbUrl.parse("typedb://admin:password@localhost/app").toOption.get
    assertEquals(u.port, 1729)
    assertEquals(u.tls, false)

  test("percent-encoded password"):
    val u = TypeDbUrl.parse("typedb://user:p%40ss@localhost/db").toOption.get
    assertEquals(u.password, "p@ss")

  test("rejects bad scheme"):
    assert(TypeDbUrl.parse("http://admin:x@localhost/db").isLeft)
