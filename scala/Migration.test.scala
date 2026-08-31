package tqlmate

import java.nio.file.Files

class MigrationTest extends munit.FunSuite:
  test("file naming"):
    val (v, n) = Migration.parseVersionName("20240101120000_create_person.tql").toOption.get
    assertEquals(v.value, "20240101120000")
    assertEquals(n, "create_person")
    assert(Migration.parseVersionName("nope.tql").isLeft)
    assert(Migration.parseVersionName("abc_name.tql").isLeft)

  test("parses sections"):
    val dir = Files.createTempDirectory("tqlmate-mig")
    val path = dir.resolve("20240101120000_demo.tql")
    Files.writeString(
      path,
      "-- migrate:up\ndefine entity x;\n\n-- migrate:down\nundefine entity x;\n"
    )
    val m = Migration.parseMigration(path).toOption.get
    assertEquals(m.up, "define entity x;")
    assertEquals(m.down, "undefine entity x;")

  test("ordering and strict"):
    def mf(v: String) =
      MigrationFile(Version(v), v, java.nio.file.Path.of(s"$v.tql"), "", "")
    val files = List(mf("1"), mf("2"), mf("3"))
    val rows = Migration.statusRows(files, List(Version("1"), Version("3")))
    assertEquals(rows(0)._2, MigrationStatus.Applied)
    assertEquals(rows(1)._2, MigrationStatus.Pending)
    assertEquals(rows(2)._2, MigrationStatus.Applied)
    assert(Migration.checkStrictOrder(files, List(Version("1"), Version("3"))).isLeft)
    assert(Migration.checkStrictOrder(files, List(Version("1"), Version("2"))).isRight)

  test("lists sorted"):
    val dir = Files.createTempDirectory("tqlmate-list")
    for name <- List("20240201000000_b.tql", "20240101000000_a.tql") do
      Files.writeString(dir.resolve(name), "-- migrate:up\n\n-- migrate:down\n")
    val files = Migration.listMigrationFiles(dir).toOption.get
    assertEquals(files(0).version.value, "20240101000000")
    assertEquals(files(1).version.value, "20240201000000")
