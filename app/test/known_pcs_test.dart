// What the phone remembers, and what the list it draws is made of.

import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/discover.dart';
import 'package:earshot/known_pcs.dart';

const _a = Destination('192.168.1.20', 47811);
const _b = Destination('192.168.1.37', 47811);

void main() {
  test('a PC that answers is remembered, newest first', () {
    final known = KnownPcs.empty.remember(_a, now: 1).remember(_b, now: 2);
    expect(known.all.first.host, _b.host);
    expect(known.all.last.host, _a.host);
  });

  test('answering again moves it to the front and keeps its name', () {
    final known = KnownPcs.empty
        .remember(_a, now: 1)
        .rename(_a, 'Windows')
        .remember(_b, now: 2)
        .remember(_a, now: 3);
    expect(known.all.first.label, 'Windows');
    expect(known.all.length, 2);
  });

  test('the same address on another port is another machine', () {
    final known = KnownPcs.empty
        .remember(_a, now: 1)
        .remember(const Destination('192.168.1.20', 47900), now: 2);
    expect(known.all.length, 2);
  });

  test('only the newest eight are kept', () {
    var known = KnownPcs.empty;
    for (var i = 0; i < 12; i++) {
      known = known.remember(Destination('192.168.1.$i', 47811), now: i);
    }
    expect(known.all.length, KnownPcs.limit);
    expect(known.all.first.host, '192.168.1.11');
  });

  test('a name can be cleared again', () {
    final known =
        KnownPcs.empty.remember(_a, now: 1).rename(_a, 'Linux').rename(_a, '  ');
    expect(known.all.single.name, isNull);
    expect(known.all.single.label, _a.host);
  });

  test('one can be forgotten, and only that one', () {
    final known = KnownPcs.empty.remember(_a, now: 1).remember(_b, now: 2);
    final after = known.forget(_a);
    expect(after.find(_a), isNull);
    expect(after.find(_b), isNotNull);
  });

  test('forgetting one that was never there changes nothing', () {
    final known = KnownPcs.empty.remember(_a, now: 1);
    expect(known.forget(_b).all.length, 1);
  });

  test('what is written is what comes back', () {
    final known = KnownPcs.empty
        .remember(_a, now: 1)
        .rename(_a, 'Windows')
        .remember(_b, now: 2);
    final back = KnownPcs.decode(known.encode());
    expect(back.all.length, 2);
    expect(back.find(_a)!.name, 'Windows');
    expect(back.find(_b)!.name, isNull);
  });

  test('a corrupted preference comes back empty rather than throwing', () {
    expect(KnownPcs.decode('not json').isEmpty, isTrue);
    expect(KnownPcs.decode('{"host":"x"}').isEmpty, isTrue);
    expect(KnownPcs.decode('[{"host":1,"port":"x"}]').isEmpty, isTrue);
    expect(KnownPcs.decode(null).isEmpty, isTrue);
  });

  group('the list the screen draws', () {
    test('a machine that is both answering and remembered appears once', () {
      final known = KnownPcs.empty.remember(_a, now: 1).rename(_a, 'Windows');
      final rows = mergePcs(
        const [Found(_a, Duration(milliseconds: 3))],
        known,
      );
      expect(rows.length, 1);
      expect(rows.single.label, 'Windows');
      expect(rows.single.live, isTrue);
    });

    test('remembered machines that are not answering stay on the list', () {
      final known = KnownPcs.empty.remember(_a, now: 1).remember(_b, now: 2);
      final rows = mergePcs(
        const [Found(_a, Duration(milliseconds: 3))],
        known,
      );
      expect(rows.length, 2);
      expect(rows.first.live, isTrue);
      expect(rows.last.destination, _b);
      expect(rows.last.live, isFalse);
    });

    test('everything answering comes before anything only remembered', () {
      final known = KnownPcs.empty.remember(_a, now: 1);
      final rows = mergePcs(
        const [Found(_b, Duration(milliseconds: 9))],
        known,
      );
      expect(rows.first.destination, _b);
      expect(rows.last.destination, _a);
    });

    test('with nothing remembered it is just what answered', () {
      final rows = mergePcs(
        const [Found(_a, Duration(milliseconds: 3))],
        KnownPcs.empty,
      );
      expect(rows.single.label, _a.host);
      expect(rows.single.known, isFalse);
    });
  });
}
