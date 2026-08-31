package tqlmate

import java.nio.file.{Files, Path}
import scala.jdk.CollectionConverters.*
import scala.util.matching.Regex

opaque type Version = String
object Version:
  def apply(s: String): Version = s
  extension (v: Version) def value: String = v
  given Ordering[Version] = Ordering[String].on(_.value)

final case class MigrationFile(
  version: Version,
  name: String,
  path: Path,
  up: String,
  down: String
):
  def label: String = s"${version.value}_$name"

enum MigrationStatus:
  case Applied, Pending

object Migration:
  private val FileName: Regex = raw"^(\d+)_(.+)\.tql$$".r
  private val UpMarker = raw"(?i)^\s*--\s*migrate:up\s*$$".r
  private val DownMarker = raw"(?i)^\s*--\s*migrate:down\s*$$".r

  def parseVersionName(filename: String): Result[(Version, String)] =
    filename match
      case FileName(ver, name) if ver.nonEmpty && name.nonEmpty =>
        Right((Version(ver), name))
      case _ =>
        Left(Error.msg(s"expected VERSION_name.tql: $filename"))

  def parseMigration(path: Path): Result[MigrationFile] =
    val filename = Option(path.getFileName).map(_.toString).getOrElse("")
    for
      (version, name) <- parseVersionName(filename)
      text <- read(path)
      (up, down) <- splitUpDown(text)
    yield MigrationFile(version, name, path, up, down)

  def listMigrationFiles(dir: Path): Result[List[MigrationFile]] =
    if !Files.exists(dir) then Right(Nil)
    else
      val paths =
        Files
          .list(dir)
          .iterator()
          .asScala
          .filter(p => Files.isRegularFile(p) && p.getFileName.toString.endsWith(".tql"))
          .toList
      paths
        .foldLeft[Result[List[MigrationFile]]](Right(Nil)): (acc, p) =>
          for
            xs <- acc
            m <- parseMigration(p)
          yield m :: xs
        .map(_.sortBy(_.version))
        .flatMap: files =>
          files.sliding(2).find(w => w.length == 2 && w(0).version == w(1).version) match
            case Some(dup) =>
              Left(Error.msg(s"duplicate migration version ${dup.head.version.value}"))
            case None => Right(files)

  def statusRows(
    files: List[MigrationFile],
    applied: List[Version]
  ): List[(MigrationFile, MigrationStatus)] =
    val set = applied.toSet
    files.map: f =>
      val st = if set(f.version) then MigrationStatus.Applied else MigrationStatus.Pending
      (f, st)

  def checkStrictOrder(files: List[MigrationFile], applied: List[Version]): Result[Unit] =
    applied.maxOption match
      case None => Right(())
      case Some(max) =>
        files.find(f => !applied.contains(f.version) && f.version.value < max.value) match
          case Some(f) =>
            Left(
              Error.msg(
                s"strict: pending migration ${f.version.value} would apply out of order (applied up to ${max.value})"
              )
            )
          case None => Right(())

  def newMigrationPath(dir: Path, name: String): Path =
    val ts = java.time.LocalDateTime
      .now(java.time.ZoneOffset.UTC)
      .format(java.time.format.DateTimeFormatter.ofPattern("yyyyMMddHHmmss"))
    dir.resolve(s"${ts}_${slugify(name)}.tql")

  def template: String = "-- migrate:up\n\n\n-- migrate:down\n\n"

  private def splitUpDown(text: String): Result[(String, String)] =
    var section: Option[String] = None
    val up = StringBuilder()
    val down = StringBuilder()
    text.linesIterator.foreach: line =>
      line match
        case UpMarker() => section = Some("up")
        case DownMarker() => section = Some("down")
        case _ =>
          section match
            case Some("up") =>
              up.append(line); up.append('\n')
            case Some("down") =>
              down.append(line); down.append('\n')
            case _ => ()
    if section.isEmpty && up.isEmpty && down.isEmpty then
      Left(Error.msg("migration missing -- migrate:up / -- migrate:down"))
    else Right((up.toString.trim, down.toString.trim))

  private def slugify(name: String): String =
    val out = StringBuilder()
    name.foreach: c =>
      if c.isLetterOrDigit then out.append(c.toLower)
      else if out.isEmpty || out.last != '_' then out.append('_')
    val trimmed = out.toString.dropWhile(_ == '_').reverse.dropWhile(_ == '_').reverse
    if trimmed.isEmpty then "migration" else trimmed

  private def read(path: Path): Result[String] =
    try Right(Files.readString(path))
    catch case e: Exception => Left(Error.Io(e))
