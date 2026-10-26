// The version appears in two files. This is the test that stops them disagreeing.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/main.dart';

void main() {
  test('the version on screen is the version in pubspec.yaml', () {
    final line = File('pubspec.yaml')
        .readAsLinesSync()
        .firstWhere((l) => l.startsWith('version:'));
    final declared = line.split(':')[1].trim().split('+').first;
    expect(
      appVersion,
      declared,
      reason: 'appVersion in lib/main.dart and version in pubspec.yaml differ',
    );
  });
}
