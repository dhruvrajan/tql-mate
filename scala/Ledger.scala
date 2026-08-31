package tqlmate

import com.typedb.driver.api.Driver
import com.typedb.driver.api.Transaction
import com.typedb.driver.api.concept.Concept

import scala.jdk.CollectionConverters.*
import scala.jdk.OptionConverters.*
import scala.util.Using

object Ledger:
  val Entity = "_tqlmate_schema_migration"
  val AttrVersion = "_tqlmate_version"
  val AttrAppliedAt = "_tqlmate_applied_at"

  private val EnsureSchema =
    s"""define
       |  attribute $AttrVersion, value string;
       |  attribute $AttrAppliedAt, value datetime;
       |  entity $Entity,
       |    owns $AttrVersion @card(1),
       |    owns $AttrAppliedAt @card(1);
       |""".stripMargin

  def ensure(driver: Driver, database: String): Result[Unit] =
    appliedVersions(driver, database) match
      case Right(_) => Right(())
      case Left(_)  => schemaQueries(driver, database, List(EnsureSchema))

  def appliedVersions(driver: Driver, database: String): Result[List[Version]] =
    edge:
      Using.resource(driver.transaction(database, Transaction.Type.READ)): tx =>
        val answer = tx.query(s"match $$m isa $Entity, has $AttrVersion $$v;").resolve()
        val versions =
          answer
            .asConceptRows()
            .asScala
            .toList
            .flatMap: row =>
              row.get("v").asScala.toList.flatMap: concept =>
                stringValue(concept).map(Version(_))
        versions.sorted

  def recordInsert(version: Version, appliedAt: String): String =
    s"""insert $$_ isa $Entity, has $AttrVersion "${version.value}", has $AttrAppliedAt $appliedAt;"""

  def recordDelete(version: Version): String =
    s"""match $$m isa $Entity, has $AttrVersion "${version.value}"; delete $$m;"""

  def schemaQueries(driver: Driver, database: String, queries: List[String]): Result[Unit] =
    edge:
      Using.resource(driver.transaction(database, Transaction.Type.SCHEMA)): tx =>
        queries.foreach: q =>
          val trimmed = q.trim
          if trimmed.nonEmpty then
            tx.query(trimmed).resolve()
        tx.commit()

  private def stringValue(concept: Concept): Option[String] =
    val opt = concept.tryGetString()
    if opt.isPresent then Some(opt.get) else None

  private def edge[A](body: => A): Result[A] =
    try Right(body)
    catch case e: Exception => Left(Error.fromThrowable(e))
